@echo off
rem Build the CoreAudio helper (no Python / no NuGet needed).
rem Requires .NET Framework csc (ships with Windows).
set CSC=%WINDIR%\Microsoft.NET\Framework64\v4.0.30319\csc.exe
if not exist "%CSC%" set CSC=%WINDIR%\Microsoft.NET\Framework\v4.0.30319\csc.exe
"%CSC%" /nologo /target:exe /out:AudioSwitch.exe AudioSwitch.cs
echo Built AudioSwitch.exe
echo Install it to: %LOCALAPPDATA%\AudioSwitch\AudioSwitch.exe
