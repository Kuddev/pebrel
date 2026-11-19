"""Run a test command with a fresh loopback-only OpenSSH identity."""
import getpass
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import time


def main(command):
    with tempfile.TemporaryDirectory(prefix="pebrel-sshd-") as path:
        root = Path(path)
        with socket.socket() as sock:
            sock.bind(("127.0.0.1", 0))
            port = sock.getsockname()[1]
        user = getpass.getuser()
        for name in ("host", "client"):
            subprocess.run(["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", root/name], check=True)
        fp = subprocess.check_output(["ssh-keygen", "-lf", root/"host.pub"], text=True).split()[1]
        config = root/"sshd_config"
        config.write_text(f"""Port {port}
ListenAddress 127.0.0.1
HostKey {root}/host
PidFile {root}/sshd.pid
AuthorizedKeysFile {root}/client.pub
StrictModes no
AllowUsers {user}
PermitRootLogin prohibit-password
PasswordAuthentication no
KbdInteractiveAuthentication no
UsePAM no
AllowTcpForwarding no
X11Forwarding no
""")
        # The server and client belong to this same test user.
        with subprocess.Popen(["/usr/sbin/sshd", "-D", "-e", "-f", config]) as server:
            try:
                for _ in range(100):
                    if server.poll() is not None:
                        raise RuntimeError("Test sshd exited during startup")
                    try:
                        with socket.create_connection(("127.0.0.1", port), timeout=.2):
                            break
                    except OSError:
                        time.sleep(.05)
                else:
                    raise TimeoutError("Loopback sshd did not listen")
                env = dict(os.environ, PEBREL_TEST_PORT=str(port), PEBREL_TEST_USER=user,
                           PEBREL_TEST_KEY_PATH=str(root/"client"), PEBREL_TEST_FINGERPRINT=fp)
                subprocess.run(command, env=env, check=True, timeout=900)
            finally:
                server.terminate()


if __name__ == "__main__":
    main(sys.argv[1:])
