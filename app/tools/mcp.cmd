@echo off
rem Starts the MCP server (section-response --mcp) from a copy of the release exe, so that
rem `cargo build --release` can replace target\release\section-response.exe while the server
rem is running. The copy is refreshed each time the server starts; if it is in use (another
rem session's server), the existing copy is used. Nothing but the server may write to stdout.
setlocal
set "APP=%~dp0.."
set "SRC=%APP%\target\release\section-response.exe"
set "DST=%APP%\target\mcp\section-response.exe"
if not exist "%APP%\target\mcp" mkdir "%APP%\target\mcp" >nul 2>&1
copy /y "%SRC%" "%DST%" >nul 2>&1
"%DST%" --mcp
