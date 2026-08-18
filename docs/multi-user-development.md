# Multi-user development on one Windows machine

Run this only on a test host with concurrent Windows user sessions (Windows Server, Azure Virtual Desktop, or a comparable multi-session setup). A typical Windows 11 desktop allows only one interactive user session at a time.

From an elevated PowerShell window in the repository:

```powershell
.\tools\dev-multiuser.ps1 -Action Start
```

The command starts one Vite server on `127.0.0.1:1420`, builds the debug Free executable, then creates temporary Scheduled Tasks:

- each user already signed in receives the debug application in their own Windows session;
- any user who signs in while the test is running receives the debug application automatically;
- the installed app is closed first, and only a user whose installed app was open is considered for restoration.

Stop the test from the same elevated shell:

```powershell
.\tools\dev-multiuser.ps1 -Action Stop
```

Stopping removes the temporary tasks, closes the debug application, stops Vite, and restores the prior installed application when that user can launch it. The release executable requires Windows elevation, so a standard Windows account cannot silently relaunch it; Windows will require administrator approval in that case. This limitation is deliberate—development tooling must not bypass Windows elevation.

This workflow is not bundled in the MSI and does not run on customer devices.
