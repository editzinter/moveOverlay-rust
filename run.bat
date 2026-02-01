@echo off
setlocal

:: 1. Setup Visual Studio Environment
call "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat"

:: 2. Manually add standard SDK paths if they are missing
:: (This fixes the LNK1181 error if vcvars didn't pick them up)
if not exist "C:\Program Files (x86)\Windows Kits\10\Lib" goto :TrySdk8

:: Try to find the latest SDK version
for /f "delims=" %%D in ('dir /b /ad /o-n "C:\Program Files (x86)\Windows Kits\10\Lib\10.*"') do (
    set "SDK_VER=%%D"
    goto :FoundSdk10
)

:FoundSdk10
echo Found SDK: %SDK_VER%
set "LIB=%LIB%;C:\Program Files (x86)\Windows Kits\10\Lib\%SDK_VER%\um\x64"
set "LIB=%LIB%;C:\Program Files (x86)\Windows Kits\10\Lib\%SDK_VER%\ucrt\x64"
goto :Run

:TrySdk8
:: Fallback logic could go here, but usually it's SDK 10 these days.

:Run
echo LIB PATH IS:
echo %LIB%

cd /d "%~dp0"
"C:\Users\Administrator\.cargo\bin\cargo.exe" run --release
pause
