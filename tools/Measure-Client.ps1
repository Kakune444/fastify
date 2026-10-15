param(
    [Parameter(Mandatory=$true)][ValidateSet('Spotify','Fastify')][string]$Client,
    [ValidateRange(3,300)][int]$Seconds=15,
    [string]$OutputPath
)

$ErrorActionPreference='Stop'
$rootName=if($Client -eq 'Spotify'){'Spotify.exe'}else{'fastify-desktop.exe'}
$samples=[Collections.Generic.List[object]]::new()
$previousCpu=@{}
$previousSample=[DateTime]::UtcNow

for($sampleIndex=0;$sampleIndex -lt $Seconds;$sampleIndex++){
    # Command lines and account information are deliberately not collected.
    $processTree=Get-CimInstance Win32_Process -Property Name,ProcessId,ParentProcessId
    $included=[Collections.Generic.HashSet[int]]::new()
    foreach($item in $processTree | Where-Object { $_.Name -eq $rootName }){
        [void]$included.Add([int]$item.ProcessId)
    }
    if($included.Count -eq 0){throw "$Client is not running."}
    do{
        $added=$false
        foreach($item in $processTree){
            if($included.Contains([int]$item.ParentProcessId) -and $included.Add([int]$item.ProcessId)){$added=$true}
        }
    }while($added)
    $now=[DateTime]::UtcNow
    $privateBytes=0L
    $workingSet=0L
    $cpuDelta=0.0
    $count=0
    foreach($processId in $included){
        $process=Get-Process -Id $processId -ErrorAction SilentlyContinue
        if(!$process){continue}
        $count++
        $privateBytes+=$process.PrivateMemorySize64
        $workingSet+=$process.WorkingSet64
        $cpu=$process.TotalProcessorTime.TotalSeconds
        if($previousCpu.ContainsKey($processId)){$cpuDelta+=[math]::Max(0,$cpu-$previousCpu[$processId])}
        $previousCpu[$processId]=$cpu
    }
    $elapsed=[math]::Max(0.001,($now-$previousSample).TotalSeconds)
    $samples.Add([pscustomobject]@{
        atUtc=$now.ToString('o');processCount=$count
        privateMiB=[math]::Round($privateBytes/1MB,2)
        workingSetMiB=[math]::Round($workingSet/1MB,2)
        cpuPercent=if($sampleIndex-eq 0){$null}else{[math]::Round(100*$cpuDelta/$elapsed/[Environment]::ProcessorCount,2)}
    })
    $previousSample=$now
    if($sampleIndex -lt ($Seconds-1)){Start-Sleep -Milliseconds 1000}
}

$privateValues=@($samples.privateMiB | Sort-Object)
$result=[pscustomobject]@{
    client=$Client;sampleCount=$samples.Count
    includesDescendants=$true
    medianPrivateMiB=$privateValues[[math]::Floor($privateValues.Count/2)]
    averageCpuPercent=[math]::Round(($samples | Where-Object { $null -ne $_.cpuPercent } | Measure-Object cpuPercent -Average).Average,2)
    note='Point-in-time process-tree measurements. Use equivalent workloads before comparing clients.'
    samples=$samples
}
if($OutputPath){$result|ConvertTo-Json -Depth 5|Set-Content -LiteralPath $OutputPath -Encoding utf8}
$result|Select-Object client,sampleCount,includesDescendants,medianPrivateMiB,averageCpuPercent,note|ConvertTo-Json
