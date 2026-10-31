param([string]$Binary='target/release/armatra.exe',[string]$OutputRoot='target/native/library-native',[string]$SavedMappingLibrary='', [string]$Pairs='[[6,5],[11,0]]', [switch]$Development)
$ErrorActionPreference='Stop'
$previousDirectory=Get-Location
$project=Split-Path $PSScriptRoot -Parent
Set-Location -LiteralPath $project
$binaryPath=(Resolve-Path -LiteralPath $Binary).Path
$binaryHash=(Get-FileHash -LiteralPath $binaryPath -Algorithm SHA256).Hash
$source=if($env:ARMATRA_BUILD_COMMIT){$env:ARMATRA_BUILD_COMMIT}else{'local'}
$run=Join-Path ([IO.Path]::GetFullPath($OutputRoot,$project)) ('work-'+[Guid]::NewGuid().ToString('N'))
$output=[IO.Path]::GetFullPath($OutputRoot,$project)
New-Item -ItemType Directory -Path $run,$output -Force|Out-Null
$previous=@{};Get-ChildItem Env:ARMATRA_*|ForEach-Object {$previous[$_.Name]=$_.Value}
$previousBacktrace=$env:RUST_BACKTRACE
$captures=[Collections.Generic.List[object]]::new()
function Clear-Environment {Get-ChildItem Env:ARMATRA_*|ForEach-Object {Remove-Item -LiteralPath ('Env:'+$_.Name)}}
function Native([string]$Name,[string]$Scenario,[string[]]$Files,[string]$Library,[string]$Motion='Wave') {
    Clear-Environment
    $env:ARMATRA_LIBRARY=$Library
    $env:ARMATRA_CAPTURE=Join-Path $run ($Name+'-capture')
    $env:ARMATRA_LOG_DIR=Join-Path $run ($Name+'-logs')
    $env:ARMATRA_CAPTURE_FRAMES='1';$env:ARMATRA_WINDOW_SIZE='1440x900';$env:ARMATRA_UI_SCALE='1'
    $env:ARMATRA_MOTION_OPEN='1';$env:ARMATRA_LIBRARY_TEST_SCENARIO=$Scenario;$env:ARMATRA_LIBRARY_TEST_MOTION=$Motion;$env:RUST_BACKTRACE='full'
    if($Files.Count -eq 0){$env:ARMATRA_REUSE_LIBRARY='1'}
    $started=Get-Date
    $arguments=@($Files|ForEach-Object {'"'+$_+'"'})
    $options=@{FilePath=$binaryPath;WorkingDirectory=$project;WindowStyle='Hidden';RedirectStandardError=(Join-Path $run ($Name+'-stderr.txt'));RedirectStandardOutput=(Join-Path $run ($Name+'-stdout.txt'));PassThru=$true}
    if($arguments.Count -gt 0){$options.ArgumentList=$arguments}
    $process=Start-Process @options
    $deadline=(Get-Date).AddMinutes(4)
    while(-not $process.WaitForExit(1000)){if((Get-Date)-gt$deadline){throw "Native input timeout: $Name (PID $($process.Id))"}}
    $process.Refresh()
    $events=@(Get-WinEvent -FilterHashtable @{LogName='Application';StartTime=$started;Id=1000,1001,1026} -ErrorAction SilentlyContinue|Where-Object {$_.Message -match 'armatra\.exe'}|Select-Object TimeCreated,Id,ProviderName,Message)
    $sessionPath=Join-Path $env:ARMATRA_CAPTURE 'session.json'
    $log=@(Get-ChildItem -LiteralPath $env:ARMATRA_LOG_DIR -Filter '*.jsonl'|ForEach-Object {Get-Content -LiteralPath $_.FullName|ForEach-Object {$_|ConvertFrom-Json}})
    $processEvidence=[ordered]@{name=$Name;pid=$process.Id;exit_code=$process.ExitCode;started=$started;finished=(Get-Date);binary_sha256=$binaryHash;windows=[Environment]::OSVersion.VersionString;windows_crash_events=$events;fresh_capture=$env:ARMATRA_CAPTURE;log_directory=$env:ARMATRA_LOG_DIR;os_input=$false}
    $processEvidence|ConvertTo-Json -Depth 12|Set-Content -LiteralPath (Join-Path $run ($Name+'-process.json')) -Encoding utf8
    if($process.ExitCode -ne 0){Get-Content -LiteralPath $options.RedirectStandardError -Tail 40;throw "Native process failed: $Name exit $($process.ExitCode)"}
    if(!(Test-Path -LiteralPath $sessionPath)){throw "No fresh native session: $Name"}
    $s=Get-Content -LiteralPath $sessionPath -Raw|ConvertFrom-Json
    $expected=if($Scenario -eq 'stress'){100}elseif($Scenario -like 'repro-*'){1}else{2}
    if($s.ui.motion.displayed_count -ne $expected -or !$s.ui.playing -or $null -ne $s.ui.motion.pending_generation){throw "Wrong activation/liveness state: $Name"}
    foreach($frame in $s.ui.motion.displayed){if(!$frame.frame_proof.gpu_readback -or $frame.frame_proof.width -le 1 -or $frame.frame_proof.height -le 1 -or $null -eq $frame.record -or $frame.generation -gt $frame.requested_generation){throw "Invalid displayed native frame: $Name"}}
    if(@($log|Where-Object {$_.event -in @('panic','gpu_uncaptured_error','fatal_frame_error') -or ($_.event -eq 'gpu_device_lost' -and $_.data.reason -ne 'Destroyed')}).Count -gt 0){throw "Panic or graphics diagnostic: $Name"}
    if(@($log|Where-Object {$_.event -eq 'preview_first_displayed_frame'}).Count -ne $expected){throw "Missing flushed first-frame diagnostic: $Name"}
    if(!$Development -and $s.capture.source_commit -ne $source){throw "Executable provenance differs from tested source: $Name"}
    if((Get-FileHash -LiteralPath $binaryPath -Algorithm SHA256).Hash -ne $binaryHash){throw 'Executable changed during native run'}
    $captures.Add([ordered]@{process=$processEvidence;session=$s})
    Write-Output "$Name : PASS ($expected actual displayed Preview activations, exit 0)"
}
try {
    Native 'repro-main-skinless' 'repro-main' @('tests/fixtures/Wave-permuted.glb','tests/fixtures/motions/Layered-preview.glb','tests/fixtures/Viewport-cube.glb') (Join-Path $run 'main-managed')
    Native 'repro-main-jump' 'repro-main' @('tests/fixtures/motions/Jump.glb','tests/fixtures/motions/Layered-preview.glb','tests/fixtures/Viewport-cube.glb') (Join-Path $run 'jump-managed') 'Jump'
    Native 'repro-medoid' 'repro-medoid' @('tests/fixtures/motions/Jump.glb','tests/fixtures/motions/Layered-preview.glb','tests/fixtures/retarget/Tall-target.glb') (Join-Path $run 'medoid-managed') 'Jump'
    Native 'no-target' 'single' @('tests/fixtures/motions/Multi-clip-wave.glb') (Join-Path $run 'no-target-managed')
    Native 'compatible' 'single' @('tests/fixtures/motions/Multi-clip-wave.glb','tests/fixtures/Armatra-demo.glb') (Join-Path $run 'compatible-managed')
    Native 'incompatible' 'single' @('tests/fixtures/motions/Multi-clip-wave.glb','tests/fixtures/Viewport-cube.glb') (Join-Path $run 'incompatible-managed')
    $stress=@('tests/fixtures/Viewport-cube.glb','tests/fixtures/Armatra-demo.glb','tests/fixtures/retarget/Multi-skin-wave-target.glb','tests/fixtures/retarget/Rest-frame-target.glb','tests/fixtures/Wave-permuted.glb','tests/fixtures/retarget/Walk-source.glb')
    $stress+=@(Get-ChildItem -LiteralPath (Join-Path $project 'tests/fixtures/motions') -Filter '*.glb' | ForEach-Object {[IO.Path]::GetRelativePath($project,$_.FullName)})
    # Include the multi-clip and layered transition fixtures.
    foreach($file in @('tests/fixtures/motions/Layered-preview.glb','tests/fixtures/motions/Multi-clip-wave.glb')){if($stress -notcontains $file){$stress+=$file}}
    $stress+=@('tests/fixtures/motions/Layered-preview.glb','tests/fixtures/Viewport-cube.glb')
    $stressLibrary=Join-Path $run 'stress-managed'
    Native 'stress-100' 'stress' $stress $stressLibrary
    Native 'restart' 'single' @() $stressLibrary
    if($SavedMappingLibrary){
        $profiles=@(Get-ChildItem -LiteralPath (Join-Path $SavedMappingLibrary 'profiles') -Filter '*.json' -Recurse|ForEach-Object {[ordered]@{path=$_.FullName;sha256=(Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash}})
        Native 'after-manual-restart' 'single' @() $SavedMappingLibrary
        $manual=$captures[$captures.Count-1].session
        if($null -eq $manual.ui.active_plan_id -or $manual.ui.rig_skins.target -ne 0 -or $manual.ui.mapping_operation.active_overrides.Count -ne 2){throw 'Manual mapping or chosen skin lost during Library previews'}
        foreach($pair in @(ConvertFrom-Json -InputObject $Pairs)){if(!@($manual.ui.mapping_operation.active_overrides|Where-Object {$_.source -eq $pair[0] -and $_.target -eq $pair[1]}).Count){throw 'Original manual joint IDs lost'}}
        foreach($profile in $profiles){if((Get-FileHash -LiteralPath $profile.path -Algorithm SHA256).Hash -ne $profile.sha256){throw 'Library Preview modified a saved mapping profile'}}
    }
    foreach($capture in $captures){$name=$capture.process.name;$dest=Join-Path $output $name;New-Item -ItemType Directory -Path $dest -Force|Out-Null;Copy-Item -LiteralPath (Join-Path $capture.process.fresh_capture 'session.json'),(Join-Path $capture.process.fresh_capture 'editor.png') -Destination $dest}
    [ordered]@{result='PASS';binary_sha256=$binaryHash;source=$captures[0].session.capture.source_commit;input='egui pointer/key events through real native winit UI; no OS input';captures=$captures;fresh_run=$run}|ConvertTo-Json -Depth 70|Set-Content -LiteralPath (Join-Path $output 'native-verification.json') -Encoding utf8
} finally {Clear-Environment;foreach($key in $previous.Keys){[Environment]::SetEnvironmentVariable($key,$previous[$key],'Process')};$env:RUST_BACKTRACE=$previousBacktrace;Set-Location -LiteralPath $previousDirectory}
