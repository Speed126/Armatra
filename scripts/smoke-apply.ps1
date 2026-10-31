param(
    [string]$Binary='target/release/armatra.exe',
    [string]$Target='tests/fixtures/retarget/Multi-skin-wave-target.glb',
    [string]$Source='tests/fixtures/motions/Multi-clip-wave.glb',
    [string]$Pairs='[[6,5],[11,0]]',
    [string]$Motion='Wave',
    [switch]$SingleSkin,
    [string]$OutputRoot='target/native/apply-native'
)
$ErrorActionPreference='Stop'
$previousDirectory=Get-Location
$project=Split-Path $PSScriptRoot -Parent
Set-Location -LiteralPath $project
$binaryPath=(Resolve-Path -LiteralPath $Binary).Path
$binaryHash=(Get-FileHash -LiteralPath $binaryPath -Algorithm SHA256).Hash
$sourcePath=(Resolve-Path -LiteralPath $Source).Path
$targetPath=(Resolve-Path -LiteralPath $Target).Path
$sourceHash=(Get-FileHash -LiteralPath $sourcePath -Algorithm SHA256).Hash
$targetHash=(Get-FileHash -LiteralPath $targetPath -Algorithm SHA256).Hash
$output=[IO.Path]::GetFullPath($(if([IO.Path]::IsPathRooted($OutputRoot)){$OutputRoot}else{Join-Path $project $OutputRoot}))
$run=Join-Path ([IO.Path]::GetFullPath($OutputRoot,$project)) ('work-'+[Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $run,$output|Out-Null
$previous=@{};Get-ChildItem Env:ARMATRA_*|ForEach-Object {$previous[$_.Name]=$_.Value}
function Clear-Environment {Get-ChildItem Env:ARMATRA_*|ForEach-Object {Remove-Item -LiteralPath ('Env:'+$_.Name)}}
$captures=[Collections.Generic.List[object]]::new()
try {
    $names=@('missing','pending','applying','applied','pose-a','pose-b','saved','restored','restored-preview','compact-pending-150','compact-error-150')
    if($SingleSkin){$names=@($names|Where-Object {$_ -notin @('missing','compact-error-150')})}
    foreach($name in $names) {
        Clear-Environment
        $env:ARMATRA_LIBRARY=Join-Path $run ($name+'-managed')
        if($name -like 'restored*') {$env:ARMATRA_LIBRARY=Join-Path $run 'saved-managed'}
        # Fresh output for every launch: a failed native run must never reuse an
        # older session or screenshot from a previous verification attempt.
        $env:ARMATRA_CAPTURE=Join-Path $run ($name+'-capture')
        $env:ARMATRA_CAPTURE_FRAMES='1';$env:ARMATRA_WINDOW_SIZE='1440x900';$env:ARMATRA_UI_SCALE='1'
        $env:ARMATRA_MAPPING_OPEN='1';$env:ARMATRA_CAPTURE_PREVIEW=$Motion
        $env:ARMATRA_APPLY_SCENARIO=$name;$env:ARMATRA_APPLY_PAIRS=$Pairs
        if($SingleSkin){$env:ARMATRA_APPLY_SINGLE_SKIN='1'}
        if($name -like 'compact-*') {
            $env:ARMATRA_WINDOW_SIZE='1280x720';$env:ARMATRA_UI_SCALE='1.5'
            $env:ARMATRA_APPLY_SCENARIO=if($name -like '*error*'){'missing'}else{'pending'}
        }
        Push-Location -LiteralPath $run
        try {
            if($name -like 'restored*') {& $binaryPath}else{& $binaryPath $sourcePath $targetPath}
            if($LASTEXITCODE -ne 0){throw "Native operation failed: $name"}
        }finally{Pop-Location}
        if(!(Test-Path -LiteralPath (Join-Path $env:ARMATRA_CAPTURE 'session.json'))){throw "Native capture did not produce a fresh session: $name"}
        $s=Get-Content -LiteralPath (Join-Path $env:ARMATRA_CAPTURE 'session.json') -Raw|ConvertFrom-Json
        $op=$s.ui.mapping_operation
        $expectedPairs=@(ConvertFrom-Json -InputObject $Pairs)
        if($s.ui.rig_skins.source -ne $null){throw "Unexpected explicit source skin: $name"}
        if($name -in @('missing','compact-error-150')) {
            if($op.phase -ne 'Failed' -or $null -ne $s.plan -or $op.message -notlike '*Select a target skin*'){throw "Missing-skin failure not reproduced: $name"}
        }else{
            if($s.ui.rig_skins.target -ne 0){throw "Selected target skin lost: $name"}
            if($name -in @('pending','compact-pending-150')) {
                if($op.phase -ne 'Pending' -or $null -ne $s.plan){throw "Confirmed edits were not pending: $name"}
            }elseif($name -eq 'applying') {
                if($op.phase -ne 'Applying' -or !$s.ui.plan_pending -or $op.request.target_skin -ne 0){throw 'Actual Apply request not in progress'}
            }else{
                if($null -eq $s.plan -or $op.active_settings.target_skin -ne 0 -or $op.active_pair[0] -ne $s.source -or $op.active_pair[1] -ne $s.target -or $op.active_pair[2] -ne 0){throw "Wrong plan activated: $name"}
                if($op.active_overrides.Count -ne $expectedPairs.Count){throw "Active override count mismatch: $name"}
                foreach($pair in $expectedPairs){if(!@($op.active_overrides|Where-Object {$_.source -eq $pair[0] -and $_.target -eq $pair[1]}).Count){throw "Original IDs not applied: $name"}}
            }
        }
        if($s.ui.pending_overrides.Count -ne $expectedPairs.Count){throw "Manual overrides lost: $name"}
        if($name -eq 'saved' -and ($op.phase -ne 'Saved' -or !(Test-Path -LiteralPath $op.saved_path))){throw 'Profile not actually saved'}
        if($name -like 'restored*' -and $s.plan_origin -ne 'Saved profile' -and $name -ne 'restored-preview'){throw 'Profile not restored after restart'}
        $captures.Add([ordered]@{name=$name;image="$name/editor.png";fresh_capture=$env:ARMATRA_CAPTURE;sha256=(Get-FileHash -LiteralPath (Join-Path $env:ARMATRA_CAPTURE 'editor.png') -Algorithm SHA256).Hash;session=$s})
        Write-Output "$name : PASS"
    }
    $a=($captures|Where-Object name -eq 'pose-a').session
    $b=($captures|Where-Object name -eq 'pose-b').session
    $restored=($captures|Where-Object name -eq 'restored-preview').session
    if($a.rendered_pose.lbs_world_blake3 -eq $b.rendered_pose.lbs_world_blake3 -or $a.rendered_pose.skinned_vertices -lt 1){throw 'Selected skin did not animate'}
    if([Math]::Abs($a.rendered_pose.time) -gt 0.001 -or [Math]::Abs($b.rendered_pose.time-0.75) -gt 0.001){throw 'Actual timeline seeks missed expected timestamps'}
    if($restored.rendered_pose.lbs_world_blake3 -ne $b.rendered_pose.lbs_world_blake3){throw 'Restored mapping produces a different target pose'}
    if($restored.ui.active_plan_id -ne $b.ui.active_plan_id){throw 'Restored compiled mapping identity changed'}
    $motionTarget=$a.rendered_pose.motion_target
    $rootA=($a.rendered_pose.joints|Where-Object node -eq $motionTarget).world
    $rootB=($b.rendered_pose.joints|Where-Object node -eq $motionTarget).world
    $maxDeformation=0.0
    for($i=0;$i -lt $a.rendered_pose.lbs_world_probes.Count;$i++) {
        $squared=0.0
        for($axis=0;$axis -lt 3;$axis++) {
            $delta=[double]$b.rendered_pose.lbs_world_probes[$i][$axis]-[double]$a.rendered_pose.lbs_world_probes[$i][$axis]-([double]$rootB[12+$axis]-[double]$rootA[12+$axis])
            $squared+=$delta*$delta
        }
        $maxDeformation=[Math]::Max($maxDeformation,[Math]::Sqrt($squared))
    }
    $scale=0.0
    foreach($joint in $a.rendered_pose.joints){for($axis=0;$axis -lt 3;$axis++){$scale=[Math]::Max($scale,[Math]::Abs([double]$joint.world[12+$axis]-[double]$rootA[12+$axis]))}}
    if($maxDeformation -le [Math]::Max(1e-8,$scale*1e-5)){throw "The mesh only translates with its root; no meaningful skin deformation ($maxDeformation)"}
    if((Get-FileHash -LiteralPath $sourcePath -Algorithm SHA256).Hash -ne $sourceHash -or (Get-FileHash -LiteralPath $targetPath -Algorithm SHA256).Hash -ne $targetHash){throw 'Original assets changed'}
    if((Get-FileHash -LiteralPath $binaryPath -Algorithm SHA256).Hash -ne $binaryHash){throw 'Executable changed during capture'}
    $build=$captures[0].session.capture
    foreach($capture in $captures){
        if($capture.session.capture.source_commit -ne $build.source_commit -or $capture.session.capture.binary_blake3 -ne $build.binary_blake3){throw 'Mixed native executable provenance'}
        $destination=Join-Path $output $capture.name
        New-Item -ItemType Directory -Force -Path $destination|Out-Null
        Copy-Item -LiteralPath (Join-Path $capture.fresh_capture 'editor.png'),(Join-Path $capture.fresh_capture 'session.json') -Destination $destination
    }
    [ordered]@{method='Actual native winit/wgpu readbacks after egui pointer/key playback through real skin, Map Joints, Confirm, Apply, timeline and Save controls. No forced outcome or OS input.';binary_sha256=(Get-FileHash -LiteralPath $binaryPath -Algorithm SHA256).Hash;source_file_sha256=$sourceHash;target_file_sha256=$targetHash;originals_unchanged=$true;captures=$captures;pose_changed=$true;max_deformation_after_root_translation=$maxDeformation;restart_pose_identical=$true;manual_os_input=$false}|ConvertTo-Json -Depth 45|Set-Content -LiteralPath (Join-Path $output 'native-verification.json') -Encoding utf8
}finally{Clear-Environment;foreach($key in $previous.Keys){[Environment]::SetEnvironmentVariable($key,$previous[$key],'Process')};Set-Location -LiteralPath $previousDirectory}
