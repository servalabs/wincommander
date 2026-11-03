"""Exercise the real launcher stream handling without opening a camera."""
from pathlib import Path
import subprocess
import unittest

SOURCE = Path(__file__).resolve().parents[2] / "src-tauri/commander-free/scripts/modules/privacy/privacy_shield.ps1"


class LauncherTests(unittest.TestCase):
    def test_native_output_does_not_crash_powershell_before_receipt(self):
        source = SOURCE.read_text(encoding="utf-8-sig")
        block = source.split("        $process = [System.Diagnostics.Process]::Start($startInfo)", 1)[1]
        block = block.split("        $logRoot =", 1)[0]
        script = r'''
$startInfo = New-Object System.Diagnostics.ProcessStartInfo
$startInfo.FileName = (Get-Command python.exe).Source
$startInfo.Arguments = '-c "import sys,time; print(1,flush=True); print(2,file=sys.stderr,flush=True); time.sleep(1)"'
$startInfo.UseShellExecute = $false
$startInfo.CreateNoWindow = $true
$startInfo.RedirectStandardInput = $true
$startInfo.RedirectStandardOutput = $true
$startInfo.RedirectStandardError = $true
$embeddedScript = ''
function Write-PrivacyShieldDebugStage { param($Stage) }
$process = [System.Diagnostics.Process]::Start($startInfo)
''' + block + r'''
Start-Sleep -Seconds 2
[Console]::Out.Write('receipt')
'''
        result = subprocess.run(
            ["powershell.exe", "-NoProfile", "-NonInteractive", "-Command", "-"],
            input=script, text=True, capture_output=True, timeout=10, check=False,
        )
        self.assertEqual(result.returncode, 0)
        self.assertEqual(result.stdout, "receipt")
        self.assertEqual(result.stderr, "")


if __name__ == "__main__":
    unittest.main()
