@echo off
rem Builds Ographis. Double-click, or run from a command line in the project folder.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0build.ps1" %*
set OG_RESULT=%ERRORLEVEL%
echo.
pause
exit /b %OG_RESULT%
