#!/usr/bin/env python3
# vim: softtabstop=4 shiftwidth=4 expandtab
"""
Shared harness for the ZFSBootMenu LUKS virtual-machine tests.

The tests boot ZFSBootMenu images built by the build container (Ubuntu 26.10,
Linux 7.3, OpenZFS 2.4.4) in QEMU, create pools with the ZFS 2.4.4 tools of
those images, and boot into a test boot environment that also runs ZFS 2.4.4.
See docs/luks/testing.rst for the full description.

Environment:
  ZBMTEST_WORK      work directory for disks, images and logs
                    (default /var/tmp/zbm-luks-test)
  ZBMTEST_IMAGE     build container image (default localhost/zbm-builder)
  ZBMTEST_RUNTIME   docker or podman (default: docker if installed)
  ZBMTEST_ACCEL     kvm or tcg (default: kvm when /dev/kvm is usable)
  ZBMTEST_REUSE     1 = reuse already built ZFSBootMenu test images
"""

import json
import os
import re
import shutil
import socket
import subprocess
import sys
import time
from pathlib import Path

import pexpect

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]
WORK = Path(os.environ.get("ZBMTEST_WORK", "/var/tmp/zbm-luks-test"))
IMAGE = os.environ.get("ZBMTEST_IMAGE", "localhost/zbm-builder")
RUNTIME = os.environ.get("ZBMTEST_RUNTIME") or ("docker" if shutil.which("docker") else "podman")
ACCEL = os.environ.get("ZBMTEST_ACCEL") or ("kvm" if os.access("/dev/kvm", os.W_OK) else "tcg")
REUSE = os.environ.get("ZBMTEST_REUSE") == "1"

OVMF_CODE = "/usr/share/OVMF/OVMF_CODE_4M.fd"
OVMF_VARS = "/usr/share/OVMF/OVMF_VARS_4M.fd"

PW = "pw"              # LUKS passphrase of most test volumes
PW_OTHER = "other"     # a second passphrase for one volume
ZFSKEY = "zfskey12"    # ZFS native-encryption passphrase
HOSTID = b"\x0c\xb1\xba\x00"   # 00bab10c

# Fast PBKDF settings for test volumes: TCG emulation is slow, and the boot
# environment unlocks volumes in parallel. Procedure tests use the defaults.
FAST_PBKDF = "--pbkdf argon2id --pbkdf-memory 65536 --iter-time 500"

ANSI = re.compile(r"\x1b\[[0-9;?]*[A-Za-z]|\x1b\][^\x07\x1b]*(\x07|\x1b\\)|\x1b[()][A-Z0-9]")


def clean(text):
    """Strip terminal escapes and carriage returns from console output."""
    return ANSI.sub("", text).replace("\r", "")


class Test:
    """Collects steps and checks of one test and writes its result."""

    def __init__(self, name):
        self.name = name
        self.fails = []
        self.passes = 0
        self.start = time.time()
        self.dir = WORK / "tests" / name
        self.dir.mkdir(parents=True, exist_ok=True)
        self._log = open(self.dir / "steps.log", "w")
        self.step(f"test {name}: ZBMTEST_WORK={WORK} image={IMAGE} accel={ACCEL}")

    def step(self, msg):
        line = f"[{time.strftime('%H:%M:%S')}] {msg}"
        print(line, flush=True)
        self._log.write(line + "\n")
        self._log.flush()

    def check(self, cond, msg):
        self.step(("ok   - " if cond else "FAIL - ") + msg)
        if cond:
            self.passes += 1
        else:
            self.fails.append(msg)
        return cond

    def note(self, msg):
        self.step("note - " + msg)

    def show(self, text, prefix="      "):
        for line in text.splitlines():
            if line.strip():
                self.step(prefix + line)

    def finish(self):
        result = {
            "test": self.name,
            "passed": self.passes,
            "failed": len(self.fails),
            "failures": self.fails,
            "seconds": int(time.time() - self.start),
            "zfs": "2.4.4",
        }
        (self.dir / "result.json").write_text(json.dumps(result, indent=2) + "\n")
        self.step("PASS" if not self.fails else f"FAIL ({len(self.fails)}): {self.fails}")
        sys.exit(0 if not self.fails else 1)


def run(cmd, **kw):
    """Run a host command, raising on failure; returns stdout."""
    kw.setdefault("check", True)
    kw.setdefault("capture_output", True)
    kw.setdefault("text", True)
    return subprocess.run(cmd, **kw).stdout


# --------------------------------------------------------------------- disks

def disk(group, index, size="384M", fresh=False):
    """Path of a sparse raw disk image, created when missing."""
    path = WORK / "disks" / group / f"disk{index}.img"
    path.parent.mkdir(parents=True, exist_ok=True)
    if fresh and path.exists():
        path.unlink()
    if not path.exists():
        with open(path, "wb") as f:
            f.truncate(_bytes(size))
    return path


def _bytes(size):
    units = {"K": 1 << 10, "M": 1 << 20, "G": 1 << 30}
    return int(size[:-1]) * units[size[-1]] if size[-1] in units else int(size)


def partition(path, parts):
    """GPT-partition a disk image: parts is a list of (label, size or None)."""
    run(["sgdisk", "-Z", str(path)])
    for i, (label, size) in enumerate(parts, 1):
        end = f"+{size}" if size else "0"
        run(["sgdisk", "-n", f"{i}:0:{end}", "-c", f"{i}:{label}", str(path)])


GPT_ESP = "C12A7328-F81F-11D2-BA4B-00A0C93EC93B"
GPT_LINUX = "0FC63DAF-8483-4772-8E79-3D69D8477DE4"


def gpt_disk(path, size, sector, parts):
    """
    Write a GPT for any logical sector size (sfdisk before util-linux 2.40 and
    sgdisk cannot write one for 4 KiB sectors into a file). parts is a list of
    (name, type GUID, first LBA, number of sectors or None for the rest).
    """
    import uuid
    import zlib
    nent, esize = 128, 128
    esec = nent * esize // sector
    last = size // sector - 1
    first_usable, last_usable = 2 + esec, last - esec - 1
    entries = bytearray(nent * esize)
    for i, (name, ptype, start, count) in enumerate(parts):
        end = last_usable if count is None else start + count - 1
        entries[i * esize:(i + 1) * esize] = (
            uuid.UUID(ptype).bytes_le + uuid.uuid4().bytes_le + start.to_bytes(8, "little")
            + end.to_bytes(8, "little") + bytes(8) + name.encode("utf-16-le").ljust(72, b"\0"))
    ecrc = zlib.crc32(entries)
    disk_guid = uuid.uuid4().bytes_le

    def header(cur, other, elba):
        h = bytearray(b"EFI PART" + (0x10000).to_bytes(4, "little") + (92).to_bytes(4, "little") + bytes(8)
                      + cur.to_bytes(8, "little") + other.to_bytes(8, "little")
                      + first_usable.to_bytes(8, "little") + last_usable.to_bytes(8, "little") + disk_guid
                      + elba.to_bytes(8, "little") + nent.to_bytes(4, "little") + esize.to_bytes(4, "little")
                      + ecrc.to_bytes(4, "little"))
        h[16:20] = zlib.crc32(h).to_bytes(4, "little")
        return bytes(h).ljust(sector, b"\0")

    mbr = bytearray(sector)
    mbr[446:462] = (b"\x00\x00\x02\x00\xee\xff\xff\xff" + (1).to_bytes(4, "little")
                    + min(last, 0xFFFFFFFF).to_bytes(4, "little"))
    mbr[510:512] = b"\x55\xaa"
    with open(path, "wb") as f:
        f.truncate(size)
        f.write(mbr)
        f.write(header(1, last, 2))
        f.write(entries)
        f.seek((last - esec) * sector)
        f.write(entries)
        f.write(header(last, 1, last - esec))


def loop_partition(path, n):
    """
    Loop device over partition n of a disk image, by offset: kernels without
    GPT support (such as some sandbox kernels) do not scan loop partitions.
    """
    info = json.loads(run(["sfdisk", "--json", str(path)]))["partitiontable"]
    p = info["partitions"][n - 1]
    ss = info.get("sectorsize", 512)
    return run(["losetup", "-f", "--show", "--offset", str(p["start"] * ss),
                "--sizelimit", str(p["size"] * ss), str(path)]).strip()


def payload_disk():
    """ext4 disk labelled PAYLOAD holding the test boot environment."""
    build_be()
    img = WORK / "payload.img"
    stamp = WORK / "payload" / "kver"
    if not img.exists() or img.stat().st_mtime < stamp.stat().st_mtime:
        if img.exists():
            img.unlink()
        run(["mkfs.ext4", "-q", "-F", "-L", "PAYLOAD", "-d", str(WORK / "payload"), str(img), "256M"])
    return img


def vfat_disk(path, label, files, size="64M", sector=512):
    """FAT disk image (whole-disk file system) holding files {name: bytes}."""
    if path.exists():
        path.unlink()
    with open(path, "wb") as f:
        f.truncate(_bytes(size))
    run(["mkfs.vfat", "-n", label, "-S", str(sector), str(path)])
    env = dict(os.environ, MTOOLS_SKIP_CHECK="1")
    for name, data in files.items():
        src = WORK / "tmp-vfat-file"
        src.write_bytes(data)
        parts = name.strip("/").split("/")
        for i in range(1, len(parts)):
            subprocess.run(["mmd", "-i", str(path), "::/" + "/".join(parts[:i])],
                           env=env, capture_output=True)
        run(["mcopy", "-o", "-i", str(path), str(src), "::/" + name.strip("/")], env=env)
        src.unlink()
    return path


# -------------------------------------------------------------------- images

def container(args, mounts=(), entrypoint=None):
    cmd = [RUNTIME, "run", "--rm"]
    for host, guest in mounts:
        cmd += ["-v", f"{host}:{guest}"]
    if entrypoint:
        cmd += ["--entrypoint", entrypoint]
    cmd += [IMAGE] + list(args)
    return subprocess.run(cmd, capture_output=True, text=True)


def hostid_file():
    path = WORK / "hostid"
    WORK.mkdir(parents=True, exist_ok=True)
    if not path.exists():
        path.write_bytes(HOSTID)
    return path


def hostid_arg():
    """
    spl_hostid= with the host ID of the test images and boot environment, for
    an image built elsewhere: the boot environment's root pool stays imported
    at power-off, so another host ID sees it as in use by another system.
    """
    v = int.from_bytes(hostid_file().read_bytes()[:4], "little")
    return f"spl_hostid=0x{v:08x}"


def build_be(force=False):
    """Build the ZFS 2.4.4 test boot environment (kernel, initramfs, rootfs)."""
    hostid_file()
    if (WORK / "payload" / "kver").exists() and not force:
        return
    r = container(["/out"], mounts=[(WORK, "/out"), (HERE / "be", "/be:ro")],
                  entrypoint="/be/build-be.sh")
    (WORK / "build-be.log").write_text(r.stdout + r.stderr)
    if r.returncode != 0:
        raise RuntimeError(f"boot environment build failed; see {WORK / 'build-be.log'}")


def be_kver():
    build_be()
    return (WORK / "payload" / "kver").read_text().strip()


def zbm_image(variant, crypttab=None, files=None, conf="", cmdline=None, efi=False, rc=None):
    """
    Build a ZFSBootMenu test image with the build container from this source
    tree. Returns a dict with paths of kernel, initrd and (if efi) EFI image.

      crypttab  text of /etc/zfsbootmenu/crypttab, or None for none
      files     {relative path in build dir: bytes} copied into the build dir
      conf      extra dracut configuration lines
      cmdline   Kernel.CommandLine of the EFI image
      rc        shell script run in the container before the build (rc.d)
    """
    bdir = WORK / "zbm" / variant
    out = {"kernel": bdir / "build" / "vmlinuz-bootmenu",
           "initrd": bdir / "build" / "initramfs-bootmenu.img",
           "efi": bdir / "build" / "vmlinuz.EFI"}
    if REUSE and out["initrd"].exists() and (not efi or out["efi"].exists()):
        return out
    if bdir.exists():
        shutil.rmtree(bdir)
    (bdir / "dracut.conf.d").mkdir(parents=True)
    shutil.copy(hostid_file(), bdir / "hostid")
    shutil.copytree(HERE / "hooks", bdir / "hooks")
    lines = ['zfsbootmenu_hook_root="/build/hooks"', 'compress="zstd"']
    if crypttab is not None:
        (bdir / "crypttab").write_text(crypttab)
        lines.append('zfsbootmenu_luks_crypttab="/build/crypttab"')
    else:
        lines.append('zfsbootmenu_luks_crypttab="/build/no-crypttab"')
    (bdir / "dracut.conf.d" / "zz-test.conf").write_text("\n".join(lines + [conf]) + "\n")
    for rel, data in (files or {}).items():
        p = bdir / rel
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_bytes(data)
    if rc:
        (bdir / "rc.d").mkdir()
        (bdir / "rc.d" / "10-test").write_text("#!/bin/sh\nset -e\n" + rc + "\n")
        (bdir / "rc.d" / "10-test").chmod(0o755)
    cfg = ["Components:", "  Enabled: true", "  Versions: false",
           "EFI:", f"  Enabled: {'true' if efi else 'false'}", "  Versions: false",
           "Kernel:", "  Version: 7.3.*"]
    if cmdline:
        cfg.append(f'  CommandLine: "{cmdline}"')
    (bdir / "config.yaml").write_text("\n".join(cfg) + "\n")
    r = container(["-V"], mounts=[(REPO, "/zbm:ro"), (bdir, "/build")])
    (bdir / "build.log").write_text(r.stdout + r.stderr)
    if r.returncode != 0 or not out["initrd"].exists():
        raise RuntimeError(f"ZFSBootMenu image {variant} failed to build; see {bdir / 'build.log'}")
    return out


# ------------------------------------------------------------------------ VM

class Swtpm:
    """A software TPM 2.0 whose state persists in a directory."""

    def __init__(self, state):
        self.state = Path(state)
        self.state.mkdir(parents=True, exist_ok=True)
        self.sock = f"/tmp/zbmtest-swtpm-{os.getpid()}.sock"
        self.pid = None

    def start(self):
        if os.path.exists(self.sock):
            os.remove(self.sock)
        pidfile = f"{self.sock}.pid"
        run(["swtpm", "socket", "--tpm2", "--tpmstate", f"dir={self.state}",
             "--ctrl", f"type=unixio,path={self.sock}", "-d", "--pid", f"file={pidfile}",
             "--flags", "startup-clear"])
        time.sleep(1)
        self.pid = int(Path(pidfile).read_text())
        return self.sock

    def stop(self):
        if self.pid:
            try:
                os.kill(self.pid, 15)
            except ProcessLookupError:
                pass
            self.pid = None

    def clear_lockout(self):
        """Reset the dictionary-attack lockout counter (owner auth is empty)."""
        port = 2400 + os.getpid() % 1000
        run(["swtpm", "socket", "--tpm2", "--tpmstate", f"dir={self.state}",
             "--server", f"type=tcp,port={port}", "--ctrl", f"type=tcp,port={port + 1}",
             "-d", "--pid", f"file={self.sock}.clr", "--flags", "startup-clear"])
        time.sleep(1)
        try:
            run(["tpm2_dictionarylockout", "-T", f"swtpm:host=127.0.0.1,port={port}", "-c"])
        finally:
            os.kill(int(Path(f"{self.sock}.clr").read_text()), 15)
            time.sleep(1)


SHELL_PROMPTS = [r"Type 'yes' in capital letters\): ", r"Enter passphrase for [^\n]*: ",
                 r"Verify passphrase: ", r"Enter any existing passphrase: ",
                 r"Enter new passphrase for key slot: ", r"RC=(\d+)\r*\n"]


class VM:
    """One QEMU virtual machine with a serial console driven by pexpect."""

    def __init__(self, test, label, disks=(), kernel=None, initrd=None, append="",
                 mem=4096, smp=2, tpm=None, ovmf_vars=None, nvme=None, usb=(),
                 net=None, extra=()):
        self.t = test
        self.label = label
        self.mon = f"/tmp/zbmtest-mon-{os.getpid()}.sock"
        self.swtpm = Swtpm(tpm) if tpm else None
        cmd = ["qemu-system-x86_64", "-accel", ACCEL, "-cpu", "max", "-m", str(mem),
               "-smp", str(smp), "-nographic", "-no-reboot",
               "-monitor", f"unix:{self.mon},server,nowait"]
        if ovmf_vars:
            cmd += ["-machine", "q35",
                    "-drive", f"if=pflash,format=raw,readonly=on,file={OVMF_CODE}",
                    "-drive", f"if=pflash,format=raw,file={ovmf_vars}"]
        if kernel:
            cmd += ["-kernel", str(kernel), "-initrd", str(initrd), "-append", append]
        for i, d in enumerate(disks):
            path, serial = (d if isinstance(d, tuple) else (d, f"d{i}"))
            if path is None:
                continue
            # the payload disk is only read, so tests running side by side can share it
            opts = "readonly=on,file.locking=off" if serial == "payload" else "discard=unmap"
            cmd += ["-drive", f"file={path},if=none,id=drv-{serial},format=raw,{opts}",
                    "-device", f"virtio-blk-pci,drive=drv-{serial},serial={serial},id=dev-{serial}"]
        if nvme:
            path, bs = nvme
            cmd += ["-drive", f"file={path},if=none,id=nv0,format=raw,discard=unmap",
                    "-device", "nvme,id=nvme0,serial=ZBMTEST0001",
                    "-device", f"nvme-ns,drive=nv0,bus=nvme0,nsid=1,logical_block_size={bs},physical_block_size={bs}"]
        cmd += ["-device", "qemu-xhci,id=xhci"]
        for path, ident in usb:
            cmd += ["-drive", f"file={path},if=none,id={ident},format=raw",
                    "-device", f"usb-storage,bus=xhci.0,drive={ident},id=dev-{ident}"]
        if net:
            cmd += ["-netdev", f"user,id=n0,{net}", "-device", "e1000,netdev=n0"]
        self.cmd = cmd + list(extra)
        self.c = None

    def start(self):
        if self.swtpm:
            sock = self.swtpm.start()
            self.cmd += ["-chardev", f"socket,id=chrtpm,path={sock}",
                         "-tpmdev", "emulator,id=tpm0,chardev=chrtpm", "-device", "tpm-tis,tpmdev=tpm0"]
        self.t.step(f"VM {self.label}: starting")
        self.c = pexpect.spawn(self.cmd[0], self.cmd[1:], encoding="utf-8",
                               codec_errors="replace", timeout=1800)
        self.c.logfile_read = open(self.t.dir / f"serial-{self.label}.log", "w")
        return self

    # pexpect pass-through
    def expect(self, patterns, timeout=1800):
        return self.c.expect(patterns, timeout=timeout)

    def sendline(self, s=""):
        self.c.sendline(s)

    def send(self, s):
        self.c.send(s)

    @property
    def before(self):
        return clean(self.c.before or "")

    @property
    def match(self):
        return self.c.match

    def monitor(self, command):
        s = socket.socket(socket.AF_UNIX)
        s.connect(self.mon)
        time.sleep(0.3)
        s.recv(65536)
        s.sendall((command + "\n").encode())
        time.sleep(0.8)
        out = s.recv(65536).decode(errors="replace")
        s.close()
        return out

    def hotplug_disk(self, path, serial):
        self.t.step(f"hot-plugging disk {serial}")
        self.monitor(f"drive_add 0 file={path},if=none,id=drv-{serial},format=raw")
        self.monitor(f"device_add virtio-blk-pci,drive=drv-{serial},serial={serial},id=dev-{serial}")

    def hotplug_usb(self, path, ident):
        self.t.step(f"inserting USB stick {ident}")
        self.monitor(f"drive_add 0 file={path},if=none,id={ident},format=raw")
        self.monitor(f"device_add usb-storage,bus=xhci.0,drive={ident},id=dev-{ident}")

    def recovery_shell(self):
        """From the import retry loop, open the recovery shell and prepare it."""
        self.expect(r"Unable to import", timeout=1800)
        self.send("\x1b")
        self.shell_ready()

    def shell_ready(self):
        self.expect(r"zfsbootmenu", timeout=600)
        time.sleep(3)
        self.sendline("stty -echo cols 250; export PS1='zbm# ' PS2=''")
        time.sleep(2)
        self.sh("true", quiet=True)

    def sh(self, cmd, t=1800, show=False, quiet=False, pw=PW):
        """Run a shell command; answer cryptsetup prompts like a user would."""
        lines = cmd.split("\n")
        lines[-1] += "; echo RC=$?"
        for line in lines:
            self.sendline(line)
            if len(lines) > 1:
                time.sleep(0.2)
        while True:
            i = self.expect(SHELL_PROMPTS, timeout=t)
            if i == 0:
                self.sendline("YES")
            elif i < 5:
                self.sendline(pw)
            else:
                break
        rc = int(self.c.match.group(1))
        out = "\n".join(l[5:] if l.startswith("zbm# ") else l
                        for l in self.before.split("\n")
                        if l.strip() and l.strip() != "zbm#")
        if not quiet:
            self.t.step(f"$ {cmd.splitlines()[0][:150]}{' ...' if len(lines) > 1 else ''} -> {rc}")
        if show:
            self.t.show(out)
        return rc, out

    def must(self, cmd, msg=None, **kw):
        rc, out = self.sh(cmd, **kw)
        if rc != 0:
            self.t.show(out)
            self.t.check(False, msg or f"command failed: {cmd[:120]}")
            raise RuntimeError(f"setup command failed (rc={rc}): {cmd}")
        return out

    def send_functions(self):
        """Define the shell helper functions of functions.sh in the shell."""
        for line in (HERE / "functions.sh").read_text().splitlines():
            if line.strip() and not line.lstrip().startswith("#"):
                self.sendline(line)
                time.sleep(0.05)
        time.sleep(1)
        self.sh("type mkluks >/dev/null", quiet=True)

    def poweroff(self):
        self.sendline("echo o > /proc/sysrq-trigger")
        try:
            self.expect(pexpect.EOF, timeout=180)
        except pexpect.TIMEOUT:
            self.kill()
        self.stop_tpm()

    def kill(self):
        if self.c and self.c.isalive():
            self.c.terminate(force=True)
        self.stop_tpm()

    def stop_tpm(self):
        if self.swtpm:
            self.swtpm.stop()


# --------------------------------------------------------- boot observation

BOOT_PROMPTS = {
    "luks": r"Passphrase \(leave empty to skip\): ",
    "zfskey": r"assphrase for '[^']+':",
    "tpm2pin": r"TPM2 PIN \(leave empty to use another method\): ",
    "gpg": r"Key file passphrase \(leave empty to use another method\): ",
    "missing": r"is missing",
    "retry": r"Unable to import",
    "status": r"ZBMTEST-STATUS-BEGIN",
    "keyload": r"unable to load key",
    "emergency": r"Dropping to an emergency shell",
    "be_ask": r"Please enter passphrase for disk|Enter passphrase for /dev/",
    "be_zfskey": r"(?:Enter passphrase for|Encrypted ZFS password for) [a-z]+/ROOT",
}


class Boot:
    """Result of following one boot from ZFSBootMenu into the boot environment."""

    def __init__(self):
        self.prompts = []
        self.zbm_status = ""
        self.zluks = ""
        self.teardown = None
        self.be = ""
        self.be_lines = {}

    def pools(self, text=None):
        """{pool: state} from zpool status output."""
        text = self.zbm_status if text is None else text
        res, pool = {}, None
        for line in text.splitlines():
            m = re.match(r"\s*pool: (\S+)", line)
            if m:
                pool = m.group(1)
            m = re.match(r"\s*state: (\S+)", line)
            if m and pool:
                res[pool] = m.group(1)
        return res


def follow_boot(vm, answers=None, on=None, be=True, timeout=2400):
    """
    Follow a boot: answer prompts, capture ZFSBootMenu's pool status and
    teardown report, then the boot environment's report.

      answers   {prompt kind: list of replies}; defaults answer LUKS with PW
                and the ZFS key with ZFSKEY
      on        {prompt kind: callback(vm)} for "missing" and "retry"
    """
    answers = {k: list(v) for k, v in (answers or {}).items()}
    on = on or {}
    b = Boot()
    kinds = ["luks", "zfskey", "tpm2pin", "gpg", "missing", "retry", "status", "keyload", "emergency"]
    while True:
        i = vm.expect([BOOT_PROMPTS[k] for k in kinds], timeout=timeout)
        kind = kinds[i]
        if kind == "status":
            break
        if kind == "keyload":
            b.prompts.append("keyload-failed")
            return b
        if kind == "emergency":
            # ZFSBootMenu found nothing to boot; the caller may open the shell
            b.prompts.append("emergency")
            vm.t.step("ZFSBootMenu dropped to its emergency shell")
            return b
        if kind in ("missing", "retry"):
            if kind in on:
                cb = on.pop(kind)
                cb(vm)
            if kind == "retry":
                vm.expect(r"seconds", timeout=120)
            continue
        b.prompts.append(kind)
        default = {"luks": PW, "zfskey": ZFSKEY}.get(kind, "")
        reply = answers[kind].pop(0) if answers.get(kind) else default
        vm.t.step(f"prompt {kind} -> {'(empty)' if reply == '' else 'answered'}")
        time.sleep(0.5)
        vm.sendline(reply)
    vm.expect(r"ZBMTEST-STATUS-END", timeout=600)
    b.zbm_status = "\n".join(l.split("ZS| ", 1)[1] for l in vm.before.splitlines() if "ZS| " in l)
    b.zluks = "\n".join(l.split("ZL| ", 1)[1] for l in vm.before.splitlines() if "ZL| " in l)
    vm.expect(r"ZBMTEST-TEARDOWN dm=\[(.*?)\] pools=\[(.*?)\] state=\[(.*?)\]", timeout=600)
    b.teardown = tuple(g.strip() for g in vm.match.groups())
    if not be:
        return b
    i = vm.expect([r"BE-INIT-START", BOOT_PROMPTS["be_ask"], BOOT_PROMPTS["be_zfskey"],
                   r"Kernel panic", r"emergency"], timeout=timeout)
    while i == 2:
        b.prompts.append("be-zfskey")
        vm.t.step("boot environment asks for the ZFS key -> answered")
        time.sleep(1)
        vm.sendline(ZFSKEY)
        i = vm.expect([r"BE-INIT-START", BOOT_PROMPTS["be_ask"], BOOT_PROMPTS["be_zfskey"],
                       r"Kernel panic", r"emergency"], timeout=timeout)
    if i != 0:
        b.prompts.append(["", "be-luks-prompt", "", "be-panic", "be-emergency"][i])
        return b
    vm.expect(r"BE-INIT-DONE", timeout=timeout)
    b.be = vm.before
    for line in b.be.splitlines():
        m = re.match(r"\s*(BE-[A-Z-]+): ?(.*)", line)
        if m:
            b.be_lines.setdefault(m.group(1), []).append(m.group(2).strip())
    try:
        vm.expect(pexpect.EOF, timeout=300)
    except pexpect.TIMEOUT:
        vm.kill()
    vm.stop_tpm()
    return b


def be_status(b):
    return "\n".join(l.split("BE| ", 1)[1] for l in b.be.splitlines() if "BE| " in l)


def be_features(b):
    """{(pool, feature): state} reported by the boot environment."""
    res = {}
    for line in b.be.splitlines():
        if "BE-F| " in line:
            parts = line.split("BE-F| ", 1)[1].split()
            if len(parts) >= 3:
                res[(parts[0], parts[1].replace("feature@", ""))] = parts[2]
    return res


def check_be(t, b, root_pool, expect_pools, expect_states=None):
    """Common checks of a boot that reached the test boot environment."""
    t.check("BE-INIT-START" not in b.prompts and bool(b.be), "kexec into the ZFS 2.4.4 boot environment")
    if not b.be:
        return
    kv = b.be_lines.get("BE-KERNEL", [""])[0]
    t.check(kv.startswith("7.3"), f"boot environment kernel {kv}")
    zv = b.be_lines.get("BE-ZFS-VERSION", [""])[0]
    t.check("zfs-2.4.4" in zv and "zfs-kmod-2.4.4" in zv, f"boot environment runs ZFS 2.4.4 ({zv})")
    root = b.be_lines.get("BE-ROOT", [""])[0]
    t.check(root.startswith(f"{root_pool}/") and " zfs " in f" {root} ", f"root file system {root}")
    st = be_status(b)
    pools = b.pools(st)
    for p in expect_pools:
        want = (expect_states or {}).get(p, "ONLINE")
        t.check(pools.get(p) == want, f"boot environment imported {p}: {pools.get(p)} (expected {want})")
    t.check(b.be_lines.get("BE-WRITE", [""])[0] == "ok", "root dataset is writable in the boot environment")
    for line in b.be_lines.get("BE-SCRUB", []):
        pool = line.split()[0]
        healthy = "is healthy" in line or "errors: No known data errors" in line
        want_healthy = (expect_states or {}).get(pool, "ONLINE") == "ONLINE"
        if want_healthy:
            t.check(healthy, f"scrub of {pool} in the boot environment: {line[len(pool):].strip()[:80]}")


def check_teardown(t, b):
    if b.teardown is None:
        t.check(False, "teardown report missing")
        return
    dm, pools, state = b.teardown
    t.check(not dm and not pools and not state,
            f"teardown exported pools and closed volumes (dm=[{dm}] pools=[{pools}] state=[{state}])")
