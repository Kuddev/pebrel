import plistlib
import struct
from pathlib import Path
import tempfile
import unittest
import zipfile

from audit_ipa import APP, BUNDLE_ID, audit


class AuditTests(unittest.TestCase):
    def fixture(self, *, cpu=0x100000c, platform=2, minimum=17 << 16, extra=None, **properties):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        path = Path(directory.name)/"fixture.ipa"
        info = dict(CFBundleIdentifier=BUNDLE_ID, CFBundleVersion="1",
                    CFBundleShortVersionString="2.5.0", MinimumOSVersion="17.0",
                    CFBundleSupportedPlatforms=["iPhoneOS"], CFBundleExecutable="Pebrel",
                    NSLocalNetworkUsageDescription="Connect locally.", NSCameraUsageDescription="Scan a code.",
                    NSBonjourServices=["_pebrel-pair._tcp"])
        command = struct.pack("<6I", 0x32, 24, platform, minimum, 26 << 16, 0)
        header = struct.pack("<8I", 0xfeedfacf, cpu, 0, 2, 1, len(command), 0, 0)
        with zipfile.ZipFile(path, "w") as ipa:
            ipa.writestr(APP+"Info.plist", plistlib.dumps(info | properties))
            ipa.writestr(APP+"Pebrel", header + command)
            if extra:
                ipa.writestr(extra, b"fixture")
        return path

    def test_unsigned_device_archive(self):
        audit(self.fixture(), "2.5.0")

    def test_reject_mismatched_binary_plist_or_payload(self):
        for case in [dict(cpu=0x1000007), dict(platform=7), dict(minimum=18 << 16),
                     dict(CFBundleIdentifier="other"), dict(CFBundleShortVersionString="2.4.0"),
                     dict(MinimumOSVersion="18.0"), dict(NSLocalNetworkUsageDescription=""),
                     dict(NSBonjourServices=[]), dict(extra=APP+"PlugIns/Extra.appex/Info.plist"),
                     dict(extra=APP+"embedded.mobileprovision"), dict(extra=APP+"_CodeSignature/CodeResources")]:
            with self.subTest(case=case):
                with self.assertRaises(ValueError):
                    audit(self.fixture(**case), "2.5.0")


if __name__ == "__main__":
    unittest.main()
