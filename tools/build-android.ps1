# Construit et signe l'APK Android d'AutoGameSoftwareSync (preversion 0.1.x).
# Usage (PowerShell) :  .\tools\build-android.ps1            -> arm64-v8a (vraies consoles)
#                       .\tools\build-android.ps1 -X86       -> + x86_64 (emulateur Android de test)
#                       .\tools\build-android.ps1 -Debug     -> APK de debogage (cle de debogage), sans signature release
# Prerequis : D:\Projets\Outils\_android (JDK 17, SDK 34, NDK 27.2, env.ps1), cargo-apk 0.10, cibles Rust
# aarch64-linux-android / x86_64-linux-android, cle de signature dans D:\Projets\Outils\_android-signing (voir README.txt).
# Rien n'est publie : les APK signes et leurs SHA-256 vont dans D:\Projets\Outils\_dist\android.
param(
    [switch]$X86,
    [switch]$NoArm,
    [switch]$Debug,
    [string]$Out = "D:\Projets\Outils\_dist\android",
    [string]$Signing = "D:\Projets\Outils\_android-signing"
)
$ErrorActionPreference = "Stop"
. D:\Projets\Outils\_android\env.ps1
$env:CARGO_TARGET_DIR = "D:\Projets\Outils\_build-android"
# cargo-apk avertit sur ANDROID_SDK_ROOT (texte sur stderr) : variable retiree pour cette session de script seulement.
Remove-Item Env:ANDROID_SDK_ROOT -ErrorAction SilentlyContinue
# cargo-apk exige une cle pour `--release` : la cle de DEBOGAGE standard du SDK (mot de passe public « android »), donnee
# par variables d'environnement (aucun chemin local dans android\Cargo.toml) ; l'APK est ensuite re-signe avec la vraie cle.
$env:CARGO_APK_RELEASE_KEYSTORE = Join-Path $env:ANDROID_USER_HOME "debug.keystore"
$env:CARGO_APK_RELEASE_KEYSTORE_PASSWORD = "android"
if (-not (Test-Path $env:CARGO_APK_RELEASE_KEYSTORE)) { throw "cle de debogage du SDK absente : $($env:CARGO_APK_RELEASE_KEYSTORE)" }
# Signataire attendu de l'APK final (certificat de la cle release) : un APK reste signe avec la cle de debogage est refuse.
$expectedSigner = "CN=AutoGameSoftwareSync, O=Epikou"
$root = Split-Path $PSScriptRoot -Parent
$android = Join-Path $root "android"
$ver = (Select-String -Path "$android\Cargo.toml" -Pattern '^version = "([^"]+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
$profile = if ($Debug) { "debug" } else { "release" }
$targets = @()
if (-not $NoArm) { $targets += @{ triple = "aarch64-linux-android"; abi = "arm64" } }
if ($X86) { $targets += @{ triple = "x86_64-linux-android"; abi = "x86_64" } }
if ($targets.Count -eq 0) { throw "aucune cible (retirer -NoArm ou ajouter -X86)" }
New-Item -ItemType Directory -Force $Out | Out-Null
$apksigner = "$env:ANDROID_HOME\build-tools\34.0.0\apksigner.bat"
$ks = Join-Path $Signing "agss-release.jks"
$pw = Join-Path $Signing "password.txt"
if (-not $Debug) {
    if (-not (Test-Path $ks) -or -not (Test-Path $pw)) { throw "cle de signature absente : $ks / $pw (voir $Signing\README.txt)" }
}
foreach ($t in $targets) {
    Write-Host "== $($t.triple) ($profile) =="
    Push-Location $android
    try {
        $cargoArgs = @("apk", "build", "--target", $t.triple)
        if (-not $Debug) { $cargoArgs += "--release" }
        $ErrorActionPreference = "Continue"
        & cargo @cargoArgs 2>&1 | ForEach-Object { "$_" }
        $ErrorActionPreference = "Stop"
        if ($LASTEXITCODE -ne 0) { throw "cargo apk build a echoue ($($t.triple))" }
    } finally { Pop-Location }
    $built = "$env:CARGO_TARGET_DIR\$profile\apk\AutoGameSoftwareSync.apk"
    if (-not (Test-Path $built)) { throw "APK introuvable : $built" }
    $final = Join-Path $Out "AutoGameSoftwareSync-android-$ver-$($t.abi)$(if ($Debug) { '-debug' }).apk"
    if ($Debug) {
        Copy-Item $built $final -Force
    } else {
        # Signature release (schemas v2 / v3 ; apksigner garde v3 seul quand minSdk >= 30 ; v1 inutile des Android 7) par-dessus
        # la signature de debogage de cargo-apk.
        $ErrorActionPreference = "Continue"
        & $apksigner sign --ks $ks --ks-pass "file:$pw" --ks-key-alias agss --v1-signing-enabled false --v2-signing-enabled true --v3-signing-enabled true --out $final $built 2>&1 | ForEach-Object { "$_" }
        if ($LASTEXITCODE -ne 0) { throw "apksigner sign a echoue" }
        $verify = & $apksigner verify --verbose --print-certs $final 2>&1 | ForEach-Object { "$_" }
        $code = $LASTEXITCODE
        $ErrorActionPreference = "Stop"
        $verify | Select-Object -First 4
        if ($code -ne 0) { throw "apksigner verify a echoue" }
        $signers = @($verify | Where-Object { $_ -match "certificate DN:" })
        if ($signers.Count -ne 1 -or $signers[0] -notmatch [regex]::Escape($expectedSigner)) { throw "signataire inattendu (attendu : $expectedSigner) : $($signers -join ' | ')" }
        $schemes = @($verify | Where-Object { $_ -match "^Verified using v[23] .*: true" } | ForEach-Object { ($_ -split " ")[2] }) -join " + "
        Write-Host "   signataire : $expectedSigner ($schemes)"
        Remove-Item "$final.idsig" -ErrorAction SilentlyContinue
    }
    $h = (Get-FileHash -Algorithm SHA256 $final).Hash.ToLower()
    "$h  $(Split-Path $final -Leaf)" | Out-File -Encoding ascii "$final.sha256"
    $size = [math]::Round((Get-Item $final).Length / 1MB, 1)
    Write-Host "-> $final ($size Mo)  SHA-256 $h"
}
