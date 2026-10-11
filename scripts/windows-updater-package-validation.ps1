$ErrorActionPreference = 'Stop'
# Expected minisign rejection exits are asserted explicitly below.
$PSNativeCommandUseErrorActionPreference = $false
$validationRoot = Join-Path $env:RUNNER_TEMP ([guid]::NewGuid().ToString())
New-Item -ItemType Directory -Path $validationRoot | Out-Null
$privateLog = Join-Path $validationRoot 'private.log'
$phase = 'key generation'
try {
    $key = Join-Path $validationRoot 'updater.key'
    $wrongKey = Join-Path $validationRoot 'wrong.key'
    foreach ($path in @($key, $wrongKey)) {
        & npm --prefix apps/desktop run tauri -- signer generate --ci -w $path *> $privateLog
        if ($LASTEXITCODE -ne 0) { throw 'Key generation failed' }
    }
    $env:TAURI_SIGNING_PRIVATE_KEY = $key
    $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ''
    $env:KOUSHI_UPDATER_PUBLIC_KEY = (Get-Content "$key.pub" -Raw).Trim()
    $phase = 'signed NSIS build'
    & npm --prefix apps/desktop run build:windows -- --updater *> $privateLog
    if ($LASTEXITCODE -ne 0) { throw 'Build failed' }
    $installers = @(Get-ChildItem 'target/x86_64-pc-windows-msvc/release/bundle/nsis' -Filter *.exe)
    if ($installers.Count -ne 1) { throw 'Expected one installer' }
    $installer = $installers[0].FullName
    $phase = 'verifier download'
    $archive = Join-Path $validationRoot 'minisign.zip'
    Invoke-WebRequest 'https://github.com/jedisct1/minisign/releases/download/0.12/minisign-0.12-win64.zip' -OutFile $archive
    if ((Get-FileHash $archive -Algorithm SHA256).Hash.ToLower() -ne '37b600344e20c19314b2e82813db2bfdcc408b77b876f7727889dbd46d539479') {
        throw 'Verifier checksum mismatch'
    }
    Expand-Archive $archive -DestinationPath $validationRoot
    $verifier = Join-Path $validationRoot 'minisign-win64/x86_64/minisign.exe'
    $public = Join-Path $validationRoot 'public.txt'
    $wrongPublic = Join-Path $validationRoot 'wrong-public.txt'
    $signature = Join-Path $validationRoot 'signature.txt'
    foreach ($pair in @(@("$key.pub", $public), @("$wrongKey.pub", $wrongPublic), @("$installer.sig", $signature))) {
        [IO.File]::WriteAllBytes($pair[1], [Convert]::FromBase64String((Get-Content $pair[0] -Raw).Trim()))
    }
    $phase = 'signature verification'
    & $verifier -V -m $installer -p $public -x $signature *> $privateLog
    if ($LASTEXITCODE -ne 0) { throw 'Valid signature rejected' }
    Write-Output 'windows_updater_package_signature=ok'
    $tampered = Join-Path $validationRoot 'tampered.exe'
    Copy-Item $installer $tampered
    $stream = [IO.File]::OpenWrite($tampered)
    try { $stream.Seek(0, [IO.SeekOrigin]::End) | Out-Null; $stream.WriteByte(1) } finally { $stream.Dispose() }
    & $verifier -V -m $tampered -p $public -x $signature *> $privateLog
    if ($LASTEXITCODE -eq 0) { throw 'Tampered package accepted' }
    Write-Output 'windows_updater_tampered_package=reject'
    & $verifier -V -m $installer -p $wrongPublic -x $signature *> $privateLog
    if ($LASTEXITCODE -eq 0) { throw 'Wrong key accepted' }
    Write-Output 'windows_updater_wrong_key=reject'
    Write-Output 'windows_updater_validation=ok'
} catch {
    # Signer output can contain private keys. Never publish the captured log.
    throw "Windows updater validation failed during $phase; private output withheld."
} finally {
    Remove-Item Env:TAURI_SIGNING_PRIVATE_KEY, Env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD, Env:KOUSHI_UPDATER_PUBLIC_KEY -ErrorAction SilentlyContinue
    Remove-Item $validationRoot -Recurse -Force
}
