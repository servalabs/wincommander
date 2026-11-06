# Disposable parent reparse probe

This machine-specific investigation checks whether a directory pinned with the
settings migration's Windows read/share flags can still be made a junction. It
creates new directories only under `D:\_cargo-target\settings-actual-user-temp-20261106`;
it never opens the installed product directory or changes the real ProgramData ACL.

On 2026-11-06, the probe succeeded while a read handle denied delete sharing and
a second handle requested `0x116`. The successful restricted-token run used a
LUA token with maximum privileges disabled, checked that Administrators was not
enabled and Users was enabled, and used a protected fixture ACL granting Users
W,RX and granting full control only to SYSTEM and Administrators.
This is not a separately signed-in standard-user test, a test against the real
ProgramData ACL, or proof that the complete product path is exploitable. A
failed attempt may reflect token privileges or filesystem policy rather than
prove safety. The program reports the Windows result and leaves its disposable
fixture in place; it does not perform cleanup or change accounts or services.

From an elevated Windows terminal in this worktree, with the named temporary
directory already present and at least 5 GiB free on D: and 12 GiB on C::

```powershell
$env:TEMP = 'D:\_cargo-target\settings-actual-user-temp-20261106'
$env:TMP = $env:TEMP
rustc +1.99.1 --edition 2021 tools/settings-parent-reparse-probe.rs -o "$env:TEMP\settings-parent-reparse-probe.exe"
$fixture = Join-Path $env:TEMP ('reparse-fixture-' + [Guid]::NewGuid().ToString('N'))
& "$env:TEMP\settings-parent-reparse-probe.exe" $fixture --restricted
```
