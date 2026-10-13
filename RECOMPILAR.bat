@echo off
chcp 65001 >nul
title Evidence Finder
cd /d "%~dp0"
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\instalar-e-iniciar.ps1" -Recompilar
if errorlevel 1 (
  echo.
  echo Algo deu errado. Leia a mensagem acima.
  pause
  exit /b 1
)
timeout /t 5 >nul
