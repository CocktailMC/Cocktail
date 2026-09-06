@echo off
setlocal EnableExtensions
set "ROOT=%~dp0"
if exist "%ROOT%web\index.html" set "COCKTAIL_WEB_ROOT=%ROOT%web"

if exist "%ProgramData%\Cocktail\" (
  cd /d "%ProgramData%\Cocktail"
) else (
  if not exist "%ROOT%data" mkdir "%ROOT%data"
  cd /d "%ROOT%"
)

echo Cocktail Manager
echo Open http://127.0.0.1:11011 in your browser.
echo.
"%ROOT%cocktail-control.exe"
if errorlevel 1 pause
