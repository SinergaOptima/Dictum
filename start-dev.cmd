@echo off
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\dev-server.ps1" start %*
