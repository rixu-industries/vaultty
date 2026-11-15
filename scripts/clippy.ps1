# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

$ErrorActionPreference = "Stop"

$needAddWorkspace = $false
if ($args -notcontains "-p" -and $args -notcontains "--package")
{
    $needAddWorkspace = $true
}

# https://stackoverflow.com/questions/41324882/how-to-run-a-powershell-script-with-verbose-output/70020655#70020655
# Set-PSDebug -Trace 2

if ($env:CARGO)
{
    $Cargo = $env:CARGO
} elseif (Get-Command "cargo" -ErrorAction SilentlyContinue)
{
    $Cargo = "cargo"
} else
{
    Write-Error "Could not find cargo in path." -ErrorAction Stop
}

if ($needAddWorkspace)
{
    & $Cargo clippy @args --workspace --all-targets --all-features --locked -- --deny warnings
} else
{
    & $Cargo clippy @args --all-targets --all-features --locked -- --deny warnings
}
# Windows PowerShell does not turn a failed native command into a script error, so pass its exit code on.
exit $LASTEXITCODE
