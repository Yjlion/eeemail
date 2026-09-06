@echo off
rem Launches the portable eeemail, installing the WebView2 runtime first if the
rem machine does not have it.
rem
rem eeemail's window is a WebView2 control. The NSIS installer carries the
rem runtime, so an installed copy never has to think about this; an unzipped
rem copy does, and without the runtime the app starts and then vanishes with no
rem message at all. Run this once, then eeemail.exe directly if you prefer.
rem
rem Shipped by .github/workflows/release.yml. See docs/PORTABLE.md.

setlocal

rem The Evergreen runtime registers under this GUID, per-machine (HKLM, and
rem WOW6432Node on 64-bit Windows) or per-user (HKCU). Any one of the three is
rem enough.
set "WV2={F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}"
set "FOUND="
for %%K in (
  "HKLM\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\%WV2%"
  "HKLM\SOFTWARE\Microsoft\EdgeUpdate\Clients\%WV2%"
  "HKCU\SOFTWARE\Microsoft\EdgeUpdate\Clients\%WV2%"
) do (
  reg query %%K /v pv >nul 2>&1 && set "FOUND=1"
)

if not defined FOUND (
  if exist "%~dp0MicrosoftEdgeWebview2Setup.exe" (
    echo Installing the Microsoft Edge WebView2 runtime, which eeemail needs to
    echo draw its window. This happens once.
    start /wait "" "%~dp0MicrosoftEdgeWebview2Setup.exe" /silent /install
  ) else (
    echo The WebView2 runtime is missing and MicrosoftEdgeWebview2Setup.exe is
    echo not in this folder. Install it from
    echo   https://developer.microsoft.com/microsoft-edge/webview2/
    echo and run eeemail.exe again.
    pause
    exit /b 1
  )
)

start "" "%~dp0eeemail.exe"
endlocal
