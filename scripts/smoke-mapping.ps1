param([string]$Binary='target/release/armatra.exe',[string]$OutputRoot='target/native/mapping-native')
$ErrorActionPreference='Stop'
$previousDirectory=Get-Location
$project=Split-Path $PSScriptRoot -Parent
Set-Location -LiteralPath $project
$binaryPath=(Resolve-Path -LiteralPath $Binary).Path
$output=[IO.Path]::GetFullPath($OutputRoot,$project)
$run=Join-Path ([IO.Path]::GetFullPath($OutputRoot,$project)) ('work-'+[Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $run,$output|Out-Null
$previous=@{};Get-ChildItem Env:ARMATRA_*|ForEach-Object {$previous[$_.Name]=$_.Value}
function Clear-Environment {Get-ChildItem Env:ARMATRA_*|ForEach-Object {Remove-Item -LiteralPath ('Env:'+$_.Name)}}
$captures=[Collections.Generic.List[object]]::new()
try {
    Clear-Environment
    $env:ARMATRA_SMOKE_SCOPE='mapping';$env:ARMATRA_REPORT_DIR=$run
    & $binaryPath --ui-smoke
    if($LASTEXITCODE -ne0){throw 'Mapping input smoke failed'}
    $proof=Get-Content -LiteralPath (Join-Path $run 'mapping-mapping-interactions.json') -Raw|ConvertFrom-Json
    $library=if([IO.Path]::IsPathRooted($proof.managed_library)){$proof.managed_library}else{Join-Path $project $proof.managed_library}
    $source=Join-Path $run 'Procedural-wave-source.glb';$target=Join-Path $run 'Proportion-target.glb'
    Copy-Item -LiteralPath (Join-Path $library ('blobs/'+$proof.source+'.glb')) -Destination $source
    Copy-Item -LiteralPath (Join-Path $library ('blobs/'+$proof.target+'.glb')) -Destination $target
    foreach($name in @('workspace','dragging','drag-confirm','button-empty','button-filled','conflict','pending','help-icons','compact-150','compact-conflict-150','help-icons-150')) {
        Clear-Environment
        $env:ARMATRA_LIBRARY=Join-Path $run ($name+'-managed')
        $env:ARMATRA_CAPTURE=Join-Path $output $name
        $env:ARMATRA_CAPTURE_FRAMES='1';$env:ARMATRA_CAPTURE_TIME='0.5';$env:ARMATRA_WINDOW_SIZE='1440x900';$env:ARMATRA_UI_SCALE='1'
        $env:ARMATRA_MAPPING_OPEN='1';$env:ARMATRA_MAPPING_JOINT=[string]$proof.source_node
        $env:ARMATRA_CAPTURE_MAPPING_SOURCE=[string]$proof.source_node;$env:ARMATRA_CAPTURE_MAPPING_TARGET=[string]$proof.target_node
        $env:ARMATRA_CAPTURE_MAPPING_SCENARIO=$name
        if($name -like 'help-icons*') {$env:ARMATRA_CAPTURE_MAPPING_SCENARIO='workspace';$env:ARMATRA_HELP_OPEN='1';$env:ARMATRA_CAPTURE_HELP_ICONS='1'}
        if($name -eq 'compact-150') {$env:ARMATRA_CAPTURE_MAPPING_SCENARIO='button-filled';$env:ARMATRA_WINDOW_SIZE='1280x720';$env:ARMATRA_UI_SCALE='1.5'}
        if($name -eq 'compact-conflict-150') {$env:ARMATRA_CAPTURE_MAPPING_SCENARIO='conflict'}
        if($name -in @('compact-conflict-150','help-icons-150')) {$env:ARMATRA_WINDOW_SIZE='1280x720';$env:ARMATRA_UI_SCALE='1.5'}
        & $binaryPath $source $target
        if($LASTEXITCODE -ne0){throw "Native capture failed: $name"}
        $session=Get-Content -LiteralPath (Join-Path $env:ARMATRA_CAPTURE 'session.json') -Raw|ConvertFrom-Json
        $op=$session.ui.mapping_edit.operation
        if($name -in @('workspace','dragging','drag-confirm','button-empty','button-filled','compact-150') -and $session.ui.pending_overrides.Count -ne 0){throw "Override created without confirmation: $name"}
        if($name -in @('drag-confirm','button-filled','compact-150') -and ($op.source -ne $proof.source_node -or $op.target -ne $proof.target_node)){throw "Original node IDs not preserved: $name"}
        if($name -eq 'dragging' -and ($op.state -ne 'drag' -or !$op.active)){throw 'Active drag not rendered'}
        if($name -in @('drag-confirm','button-filled','conflict','compact-150') -and ($op.state -ne 'confirmation' -or $null -eq $op.source -or $null -eq $op.target)){throw "Confirmation not populated: $name"}
        if($name -eq 'button-empty' -and ($null -ne $op.source -or $null -ne $op.target -or $session.ui.mapping_confirm_enabled)){throw 'Empty panel not empty/disabled'}
        if($name -eq 'conflict' -and ($session.ui.mapping_confirm_enabled -or !$session.ui.mapping_controls.replace)){throw 'Conflict did not require resolution'}
        if($name -eq 'pending' -and (!$session.ui.plan_dirty -or $session.ui.pending_overrides.Count -ne 1)){throw 'Confirmed mapping not pending'}
        if($name -eq 'compact-150' -and $session.capture.ui_scale -ne 1.5){throw 'Incorrect compact scale'}
        $captures.Add([ordered]@{name=$name;image="$name/editor.png";sha256=(Get-FileHash -LiteralPath (Join-Path $env:ARMATRA_CAPTURE 'editor.png') -Algorithm SHA256).Hash;session=$session})
        Write-Output "$name : PASS"
    }
    [ordered]@{method='Native winit/wgpu surface readbacks after capture-only egui pointer event playback. Help scroll is capture-configured. No OS SendInput and no compositing.';date_utc=[DateTime]::UtcNow.ToString('o');binary_sha256=(Get-FileHash -LiteralPath $binaryPath -Algorithm SHA256).Hash;fixtures='Original generated MIT humanoids; source joint renamed mixamorig_LeftArm; target node IDs permuted.';source_hash=$proof.source;target_hash=$proof.target;captures=$captures;manual_os_pointer_input=$false;os_input_scope='Physical OS mouse/focus input is not measured by this synthetic egui driver.'}|ConvertTo-Json -Depth 30|Set-Content -LiteralPath (Join-Path $output 'native-verification.json') -Encoding utf8
}finally{Clear-Environment;foreach($key in $previous.Keys){[Environment]::SetEnvironmentVariable($key,$previous[$key],'Process')};Set-Location -LiteralPath $previousDirectory}
